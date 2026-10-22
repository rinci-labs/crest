//! Files dragged in and out of the shelf, and the share zone.

use super::*;

impl App {
    pub(super) fn shelf_changed(&mut self) {
        self.shelf.save();
        let shelf = &self.shelf;
        self.renderer.retain_thumbnails(|id| shelf.get(id).is_some());
    }

    pub(super) fn on_drag(&mut self, kind: usize, lparam: LPARAM) {
        match kind {
            DRAG_OVER => {
                // A drop area only lights up when the file is right over it.
                let (x, y) = point(lparam);
                let (pos, scale) = (self.window_pos(), self.renderer.scale());
                let hit = self.renderer.hit_at((x - pos.x) as f32 / scale, (y - pos.y) as f32 / scale);
                let over = match hit {
                    Some(Hit::DropShare) => Some(DropZone::Share),
                    Some(Hit::ShelfStrip | Hit::ShelfItem(_) | Hit::ShelfRemove(_)) => Some(DropZone::Shelf),
                    _ => None,
                };
                if over == self.drop_over {
                    return;
                }
                self.drop_over = over;
            }
            DRAG_ENTER => {
                // A hidden shelf does not accept files.
                if !self.settings.pages.shown(Page::Shelf) {
                    return;
                }
                self.shelf.drop_hover = true;
                self.kill_timer(TIMER_CLOSE);
                if self.expanded {
                    self.go_page(Page::Shelf, self.page_dir(Page::Shelf));
                } else {
                    self.page = Page::Shelf;
                    self.page_anim = None;
                    self.set_expanded(true);
                }
            }
            DRAG_LEAVE => {
                self.shelf.drop_hover = false;
                self.drop_over = None;
                if self.expanded && !self.hovering {
                    self.set_timer(TIMER_CLOSE, CLOSE_DELAY_MS);
                }
            }
            DRAG_DROP => {
                self.shelf.drop_hover = false;
                let dropped = dnd::take_dropped();
                // Dropped on the Share zone: open the Windows Share window, not the shelf.
                if self.drop_over.take() == Some(DropZone::Share) {
                    self.share_files(&dropped);
                    self.refresh();
                    return;
                }
                let added = if self.settings.pages.shown(Page::Shelf) { self.shelf.add(dropped) } else { Vec::new() };
                if !added.is_empty() {
                    self.shelf.save();
                    self.start_motion();
                    self.drop_glow = Some(self.now());
                    self.pop_tiles(&added);
                    self.services.thumbnails(added);
                }
                if !self.hovering {
                    self.set_timer(TIMER_CLOSE, DROP_CLOSE_MS);
                }
            }
            _ => {}
        }
        self.refresh();
    }

    /// Open the Windows Share window for files dropped on the Share zone.
    pub(super) fn share_files(&mut self, paths: &[std::path::PathBuf]) {
        if self.share.is_none() {
            self.share = share::Share::new(self.hwnd).inspect_err(|e| error_log!("Share window unavailable: {e}")).ok();
        }
        let Some(share) = &self.share else { return };
        // Windows only shows the Share window for the foreground window, while Crest is
        // normally never active. Once the window appears, Crest goes back to normal.
        self.set_activatable(true);
        bring_to_front(self.hwnd);
        if let Err(e) = share.show(paths) {
            error_log!("failed to open Share window: {e}");
        }
        self.set_activatable(false);
    }

    /// New tiles pop in, growing.
    pub(super) fn pop_tiles(&mut self, added: &[(u64, std::path::PathBuf)]) {
        self.start_motion();
        let now = self.now();
        for (id, _) in added {
            self.tile_fx.insert(*id, (now, false));
        }
    }

    /// A tile starts fading out; it is removed from the shelf in `tick` once the animation ends.
    pub(super) fn fade_tile(&mut self, id: u64) {
        if self.tile_fx.get(&id).is_some_and(|(_, removing)| *removing) {
            return;
        }
        self.start_motion();
        let now = self.now();
        self.tile_fx.insert(id, (now, true));
    }

    pub(super) fn drag_path(&self) -> Option<std::path::PathBuf> {
        self.drag_item.and_then(|id| self.shelf.get(id)).map(|item| item.path.clone())
    }

    pub(super) fn after_drag(&mut self) {
        // A file moved elsewhere no longer exists at its old location.
        if let Some(id) = self.drag_item.take()
            && self.shelf.get(id).is_some_and(|item| !item.path.exists())
        {
            self.shelf.remove(id);
            self.shelf_changed();
        }
        // The cursor left during the drag.
        self.hovering = false;
        self.tracking_leave = false;
        if self.expanded {
            self.set_timer(TIMER_CLOSE, CLOSE_DELAY_MS);
        }
        self.refresh();
    }
}
