//! Package identity for reading message notifications (optional). Windows only grants
//! notification access to apps with package identity. Crest gets it through a "sparse" package:
//! a small package holding only a manifest and logos (no program) that points to crest.exe in
//! the install folder.
//!
//! The package must be signed with a certificate Windows trusts. Without a paid certificate,
//! Crest creates one on this computer, in a single step that needs administrator rights:
//! 1. a new RSA key is created in the Windows key store under a random name,
//! 2. a "CN=Crest Local" certificate is created with that key and used to sign the package,
//!    which is built on the spot from the manifest and logos embedded in crest.exe,
//! 3. the key is deleted right away (also if any step fails), so the certificate can never sign
//!    anything again,
//! 4. the certificate (without its key) is trusted in LocalMachine\TrustedPeople.
//!
//! That administrator step takes no outside input (no file or certificate is read), so other
//! programs can't use it to get something trusted. The package is then registered for the
//! current user, without administrator rights.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use windows::Foundation::Uri;
use windows::Management::Deployment::{AddPackageOptions, PackageManager};
use windows::Win32::Foundation::{HANDLE, HWND, SYSTEMTIME};
use windows::Win32::Security::Cryptography::{
    BCRYPT_RSA_ALGORITHM, CALG_SHA_256, CERT_BASIC_CONSTRAINTS2_INFO, CERT_CONTEXT,
    CERT_CREATE_SELFSIGN_FLAGS, CERT_EXTENSION, CERT_EXTENSIONS, CERT_FIND_SUBJECT_STR,
    CERT_KEY_SPEC, CERT_OPEN_STORE_FLAGS, CRYPT_KEY_PROV_INFO,
    CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_REPLACE_EXISTING, CERT_STORE_PROV_SYSTEM_W,
    CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_LOCAL_MACHINE, CERT_X500_NAME_STR,
    CRYPT_ALGORITHM_IDENTIFIER, CRYPT_ENCODE_OBJECT_FLAGS, CRYPT_INTEGER_BLOB, CTL_USAGE,
    CertAddEncodedCertificateToStore, CertCloseStore, CertCreateSelfSignCertificate,
    CertDeleteCertificateFromStore, CertFindCertificateInStore,
    CertFreeCertificateContext, CertOpenStore, CertStrToNameW,
    CryptEncodeObjectEx, HCERTSTORE, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, MS_KEY_STORAGE_PROVIDER,
    NCRYPT_FLAGS, NCRYPT_HANDLE, NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PROV_HANDLE,
    NCryptCreatePersistedKey, NCryptDeleteKey, NCryptFinalizeKey, NCryptFreeObject, NCryptOpenStorageProvider,
    NCryptSetProperty, SIGNER_CERT, SIGNER_CERT_0, SIGNER_CERT_POLICY_CHAIN_NO_ROOT,
    SIGNER_CERT_STORE, SIGNER_CERT_STORE_INFO, SIGNER_CONTEXT, SIGNER_FILE_INFO, SIGNER_NO_ATTR,
    SIGNER_SIGN_FLAGS, SIGNER_SIGNATURE_INFO, SIGNER_SUBJECT_FILE, SIGNER_SUBJECT_INFO,
    SIGNER_SUBJECT_INFO_0, SignerFreeSignerContext, SignerSignEx2, X509_ASN_ENCODING,
    X509_BASIC_CONSTRAINTS2, X509_ENHANCED_KEY_USAGE, szOID_BASIC_CONSTRAINTS2,
    szOID_ENHANCED_KEY_USAGE, szOID_PKIX_KP_CODE_SIGNING, szOID_RSA_SHA256RSA,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::Storage::Packaging::Appx::{
    APPX_COMPRESSION_OPTION_NORMAL, APPX_PACKAGE_SETTINGS, AppxFactory, IAppxFactory,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CreateUri, STGM_SHARE_EXCLUSIVE, STGM_WRITE, Uri_CREATE_CANONICALIZE,
};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::Win32::UI::Shell::{SHCreateMemStream, SHCreateStreamOnFileEx};
use windows::core::{Error, HSTRING, IUnknown, Interface, PCSTR, PCWSTR, PSTR, PWSTR, Result, w};

/// Package publisher; matches `publisher` in packaging/crest.exe.manifest and in the package manifest.
pub const SUBJECT: &str = "CN=Crest Local";
const PACKAGE_NAME: &str = "Crest";
const MANIFEST: &str = include_str!("../../packaging/package/AppxManifest.xml");
/// The signed package, in the install folder.
pub const PACKAGE_FILE: &str = "identity.msix";
/// Temporary copy of Crest that registers the package. Crest running from the install folder
/// may hold the old package identity, so it must not replace that package itself.
pub const REGISTER_COPY: &str = "crest-register-identity.exe";
/// Logos referenced by the manifest; included in the package and in the install folder.
pub const ASSETS: [(&str, &[u8]); 3] = [
    ("Square44x44Logo.png", include_bytes!("../../packaging/package/assets/Square44x44Logo.png")),
    ("Square150x150Logo.png", include_bytes!("../../packaging/package/assets/Square150x150Logo.png")),
    ("StoreLogo.png", include_bytes!("../../packaging/package/assets/StoreLogo.png")),
];

/// Administrator step: builds the package in this crest.exe's folder, signs it with a single-use
/// key, then trusts the certificate. Reads no outside input.
pub fn prepare() -> Result<()> {
    let dir = exe_dir()?;
    write_assets(&dir).map_err(io_error)?;
    // The package is built and signed in the Windows Temp folder under a random name. Files an
    // administrator process creates there can't be changed or deleted by regular programs, so
    // the package can't be swapped before it is signed. Only afterwards is it copied to the
    // install folder (Windows rejects a signed package that was modified).
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let work = windows_temp()?.join(format!("crest-identity-{}-{nanos:x}.msix", std::process::id()));
    let result = (|| {
        build_package(&work, &manifest(crate::shell::install::VERSION))?;
        let key = Key::new()?;
        let cert = Certificate::new(&key)?;
        sign(&work, cert.0)?;
        let encoded = cert.encoded();
        // The key is deleted before the certificate is trusted.
        drop(cert);
        key.delete()?;
        std::fs::copy(&work, dir.join(PACKAGE_FILE)).map_err(io_error)?;
        trust(&encoded)
    })();
    let _ = std::fs::remove_file(&work);
    result
}

/// `C:\Windows\Temp`, from the Windows API (not from environment variables the user can change).
fn windows_temp() -> Result<PathBuf> {
    let mut buffer = [0u16; 260];
    let len = unsafe { windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(Error::from_thread());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&buffer[..len])).join("Temp"))
}

fn io_error(e: std::io::Error) -> Error {
    Error::new(windows::Win32::Foundation::E_FAIL, e.to_string())
}

/// Registers the package in `dir` for the current user, pointing to crest.exe in the same folder.
/// An older package with the same name is replaced.
pub fn register(dir: &Path) -> Result<()> {
    let manager = PackageManager::new()?;
    let options = AddPackageOptions::new()?;
    options.SetExternalLocationUri(&Uri::CreateUri(&HSTRING::from(file_uri(dir)))?)?;
    options.SetForceUpdateFromAnyVersion(true)?;
    let package = Uri::CreateUri(&HSTRING::from(file_uri(&dir.join(PACKAGE_FILE))))?;
    let result = manager.AddPackageByUriAsync(&package, &options)?.join()?;
    let code = result.ExtendedErrorCode()?;
    if code.is_err() {
        return Err(Error::new(code, result.ErrorText()?.to_string()));
    }
    Ok(())
}

/// Removes the current user's identity packages, but only those pointing to `dir` (the install
/// folder being uninstalled). Packages for Crest in other folders are left alone.
pub fn remove_package(dir: &Path) -> Result<()> {
    let manager = PackageManager::new()?;
    let packages = manager.FindPackagesByUserSecurityIdNamePublisher(&HSTRING::new(), &HSTRING::from(PACKAGE_NAME), &HSTRING::from(SUBJECT))?;
    for package in packages {
        // Older Windows without external location info: safer to leave it.
        let Ok(location) = package.EffectiveExternalPath() else { continue };
        let ours = location.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(dir.to_string_lossy().trim_end_matches('\\'));
        if !ours {
            continue;
        }
        let name = package.Id()?.FullName()?;
        let result = manager.RemovePackageAsync(&name)?.join()?;
        result.ExtendedErrorCode()?.ok()?;
    }
    Ok(())
}

/// A Crest identity package still exists for this user (e.g. Crest in another folder).
pub fn any_package() -> bool {
    let found = PackageManager::new().and_then(|manager| {
        manager.FindPackagesByUserSecurityIdNamePublisher(&HSTRING::new(), &HSTRING::from(PACKAGE_NAME), &HSTRING::from(SUBJECT))
    });
    found.is_ok_and(|packages| packages.First().is_ok_and(|first| first.HasCurrent().unwrap_or(false)))
}

/// Crest's certificate is already trusted on this computer.
pub fn certificate_trusted() -> bool {
    let Ok(store) = Store::open(true) else { return false };
    let found = store.find(None);
    if !found.is_null() {
        let _ = unsafe { CertFreeCertificateContext(Some(found)) };
    }
    !found.is_null()
}

/// Administrator step on uninstall: removes all Crest certificates from the trusted list.
pub fn remove_certificates() -> Result<()> {
    let store = Store::open(false)?;
    loop {
        let found = store.find(None);
        if found.is_null() {
            return Ok(());
        }
        // CertDeleteCertificateFromStore always frees the given context.
        unsafe { CertDeleteCertificateFromStore(found)? };
    }
}

/// Puts the logos referenced by the manifest in the install folder (Windows reads them there).
pub fn write_assets(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("assets"))?;
    for (name, bytes) in ASSETS {
        std::fs::write(dir.join("assets").join(name), bytes)?;
    }
    Ok(())
}

fn exe_dir() -> Result<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .ok_or_else(|| Error::new(windows::Win32::Foundation::E_FAIL, "crest.exe folder is unknown"))
}

/// Package manifest with this Crest version ("0.1.0" → "0.1.0.0"), so a new package always
/// replaces the old one.
fn manifest(version: &str) -> String {
    let mut parts: Vec<&str> = version.split('.').collect();
    parts.resize(4, "0");
    let version = parts.join(".");
    let Some(start) = MANIFEST.find("<Identity") else { return MANIFEST.to_string() };
    let identity_end = MANIFEST[start..].find("/>").map_or(MANIFEST.len(), |i| start + i);
    let Some(attr) = MANIFEST[start..identity_end].find("Version=\"").map(|i| start + i + "Version=\"".len()) else {
        return MANIFEST.to_string();
    };
    let Some(close) = MANIFEST[attr..].find('"').map(|i| attr + i) else { return MANIFEST.to_string() };
    format!("{}{version}{}", &MANIFEST[..attr], &MANIFEST[close..])
}

/// "file:///C:/Users/…/Crest" for the Windows packaging API.
fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('%', "%25").replace(' ', "%20").replace('#', "%23").replace('\\', "/");
    format!("file:///{}", text.trim_start_matches('/'))
}

/// Package holding the manifest and logos, built with the Windows packaging API.
fn build_package(path: &Path, manifest: &str) -> Result<()> {
    unsafe {
        let factory: IAppxFactory = CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER)?;
        // New file only (fails if it exists), with no shared access while it is written.
        let output = SHCreateStreamOnFileEx(
            &HSTRING::from(path.as_os_str()),
            (STGM_WRITE | STGM_SHARE_EXCLUSIVE).0,
            FILE_ATTRIBUTE_NORMAL.0,
            true,
            None::<&windows::Win32::System::Com::IStream>,
        )?;
        let hash = CreateUri(w!("http://www.w3.org/2001/04/xmlenc#sha256"), Uri_CREATE_CANONICALIZE, None)?;
        let mut settings = APPX_PACKAGE_SETTINGS { forceZip32: false.into(), hashMethod: ManuallyDrop::new(Some(hash)) };
        let written = (|| {
            let writer = factory.CreatePackageWriter(&output, &settings)?;
            for (name, bytes) in ASSETS {
                let stream = SHCreateMemStream(Some(bytes)).ok_or_else(Error::empty)?;
                let name = HSTRING::from(format!("assets\\{name}"));
                writer.AddPayloadFile(&name, w!("image/png"), APPX_COMPRESSION_OPTION_NORMAL, &stream)?;
            }
            let manifest = SHCreateMemStream(Some(manifest.as_bytes())).ok_or_else(Error::empty)?;
            writer.Close(&manifest)
        })();
        ManuallyDrop::drop(&mut settings.hashMethod);
        written
    }
}

/// Single-use RSA key in the Windows key store (the Windows package signer only accepts named
/// keys). The key is always deleted again: through `delete`, or on drop if a step fails midway.
struct Key {
    handle: NCRYPT_KEY_HANDLE,
    name: HSTRING,
}

impl Key {
    fn new() -> Result<Self> {
        // Random name so it never collides with another key.
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let name = HSTRING::from(format!("Crest-{}-{nanos:x}", std::process::id()));
        unsafe {
            let mut provider = NCRYPT_PROV_HANDLE::default();
            NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0)?;
            let mut handle = NCRYPT_KEY_HANDLE::default();
            let created = NCryptCreatePersistedKey(provider, &mut handle, BCRYPT_RSA_ALGORITHM, &name, CERT_KEY_SPEC(0), NCRYPT_FLAGS(0));
            let _ = NCryptFreeObject(NCRYPT_HANDLE(provider.0));
            created?;
            let key = Self { handle, name };
            NCryptSetProperty(NCRYPT_HANDLE(handle.0), NCRYPT_LENGTH_PROPERTY, &3072u32.to_le_bytes(), NCRYPT_FLAGS(0))?;
            NCryptFinalizeKey(handle, NCRYPT_FLAGS(0))?;
            Ok(key)
        }
    }

    /// Deletes the key from the Windows key store.
    fn delete(self) -> Result<()> {
        let handle = self.handle;
        std::mem::forget(self);
        // NCryptDeleteKey also frees the handle.
        unsafe { NCryptDeleteKey(handle, 0) }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        let _ = unsafe { NCryptDeleteKey(self.handle, 0) };
    }
}

/// "CN=Crest Local" code signing certificate with a single-use key. Must be freed before the key
/// is deleted.
struct Certificate(*const CERT_CONTEXT);

impl Certificate {
    fn new(key: &Key) -> Result<Self> {
        unsafe {
            let subject = HSTRING::from(SUBJECT);
            let mut size = 0u32;
            CertStrToNameW(X509_ASN_ENCODING, &subject, CERT_X500_NAME_STR, None, None, &mut size, None)?;
            let mut name = vec![0u8; size as usize];
            CertStrToNameW(X509_ASN_ENCODING, &subject, CERT_X500_NAME_STR, None, Some(name.as_mut_ptr()), &mut size, None)?;
            let name_blob = CRYPT_INTEGER_BLOB { cbData: size, pbData: name.as_mut_ptr() };

            // Code signing only, and not a certificate authority (CA).
            let mut purposes = [PSTR(szOID_PKIX_KP_CODE_SIGNING.0.cast_mut())];
            let usage = CTL_USAGE { cUsageIdentifier: 1, rgpszUsageIdentifier: purposes.as_mut_ptr() };
            let mut usage_der = encode(X509_ENHANCED_KEY_USAGE, (&raw const usage).cast())?;
            let basic = CERT_BASIC_CONSTRAINTS2_INFO { fCA: false.into(), fPathLenConstraint: false.into(), dwPathLenConstraint: 0 };
            let mut basic_der = encode(X509_BASIC_CONSTRAINTS2, (&raw const basic).cast())?;
            let mut extensions = [
                CERT_EXTENSION { pszObjId: PSTR(szOID_ENHANCED_KEY_USAGE.0.cast_mut()), fCritical: false.into(), Value: blob(&mut usage_der) },
                CERT_EXTENSION { pszObjId: PSTR(szOID_BASIC_CONSTRAINTS2.0.cast_mut()), fCritical: true.into(), Value: blob(&mut basic_der) },
            ];
            let list = CERT_EXTENSIONS { cExtension: extensions.len() as u32, rgExtension: extensions.as_mut_ptr() };
            let algorithm = CRYPT_ALGORITHM_IDENTIFIER { pszObjId: PSTR(szOID_RSA_SHA256RSA.0.cast_mut()), Parameters: CRYPT_INTEGER_BLOB::default() };

            // Valid from a year ago (so a skewed clock doesn't make it "not yet valid") until
            // 20 years from now.
            let now = GetSystemTime();
            let at = |years: i32| SYSTEMTIME { wYear: (now.wYear as i32 + years) as u16, wDay: now.wDay.min(28), ..now };
            let (start, end) = (at(-1), at(20));
            // Private key location recorded in the certificate, so the signer can use it.
            let provider = HSTRING::from(MS_KEY_STORAGE_PROVIDER.to_string().unwrap_or_default());
            let key_info = CRYPT_KEY_PROV_INFO {
                pwszContainerName: PWSTR(key.name.as_ptr().cast_mut()),
                pwszProvName: PWSTR(provider.as_ptr().cast_mut()),
                ..Default::default()
            };
            let context = CertCreateSelfSignCertificate(
                Some(HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(key.handle.0)),
                &name_blob,
                CERT_CREATE_SELFSIGN_FLAGS(0),
                Some(&key_info),
                Some(&algorithm),
                Some(&start),
                Some(&end),
                Some(&list),
            );
            if context.is_null() {
                return Err(Error::from_thread());
            }
            Ok(Self(context))
        }
    }

    /// The encoded certificate (public part only).
    fn encoded(&self) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts((*self.0).pbCertEncoded, (*self.0).cbCertEncoded as usize).to_vec() }
    }
}

impl Drop for Certificate {
    fn drop(&mut self) {
        let _ = unsafe { CertFreeCertificateContext(Some(self.0)) };
    }
}

fn encode(kind: PCSTR, value: *const c_void) -> Result<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        CryptEncodeObjectEx(X509_ASN_ENCODING, kind, value, CRYPT_ENCODE_OBJECT_FLAGS(0), None, None, &mut size)?;
        let mut bytes = vec![0u8; size as usize];
        CryptEncodeObjectEx(X509_ASN_ENCODING, kind, value, CRYPT_ENCODE_OBJECT_FLAGS(0), None, Some(bytes.as_mut_ptr().cast()), &mut size)?;
        bytes.truncate(size as usize);
        Ok(bytes)
    }
}

fn blob(bytes: &mut [u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_mut_ptr() }
}

/// SIGNER_SIGN_EX2_PARAMS and APPX_SIP_CLIENT_DATA from mssign32.h / appxsip.h (not yet in the
/// `windows` crate): extra data Windows needs to sign an MSIX package.
#[repr(C)]
struct SignerSignEx2Params {
    flags: u32,
    subject: *const SIGNER_SUBJECT_INFO,
    cert: *const SIGNER_CERT,
    signature: *const SIGNER_SIGNATURE_INFO,
    provider: *const c_void,
    timestamp_flags: u32,
    algorithm_oid: PCSTR,
    timestamp_url: PCWSTR,
    attributes: *const c_void,
    sip_data: *const c_void,
    context: *mut *mut SIGNER_CONTEXT,
    crypto_policy: *const c_void,
    reserved: *const c_void,
}

#[repr(C)]
struct AppxSipClientData {
    params: *mut SignerSignEx2Params,
    state: *mut c_void,
}

/// Signs the package with SHA-256, like `signtool sign /fd SHA256`.
fn sign(path: &Path, cert: *const CERT_CONTEXT) -> Result<()> {
    unsafe {
        let file = HSTRING::from(path.as_os_str());
        let mut index = 0u32;
        let mut file_info = SIGNER_FILE_INFO {
            cbSize: size_of::<SIGNER_FILE_INFO>() as u32,
            pwszFileName: PCWSTR(file.as_ptr()),
            hFile: HANDLE::default(),
        };
        let subject = SIGNER_SUBJECT_INFO {
            cbSize: size_of::<SIGNER_SUBJECT_INFO>() as u32,
            pdwIndex: &mut index,
            dwSubjectChoice: SIGNER_SUBJECT_FILE,
            Anonymous: SIGNER_SUBJECT_INFO_0 { pSignerFileInfo: &mut file_info },
        };
        let mut store_info = SIGNER_CERT_STORE_INFO {
            cbSize: size_of::<SIGNER_CERT_STORE_INFO>() as u32,
            pSigningCert: cert,
            dwCertPolicy: SIGNER_CERT_POLICY_CHAIN_NO_ROOT,
            hCertStore: HCERTSTORE::default(),
        };
        let signer = SIGNER_CERT {
            cbSize: size_of::<SIGNER_CERT>() as u32,
            dwCertChoice: SIGNER_CERT_STORE,
            Anonymous: SIGNER_CERT_0 { pCertStoreInfo: &mut store_info },
            hwnd: HWND::default(),
        };
        let signature = SIGNER_SIGNATURE_INFO {
            cbSize: size_of::<SIGNER_SIGNATURE_INFO>() as u32,
            algidHash: CALG_SHA_256,
            dwAttrChoice: SIGNER_NO_ATTR,
            ..Default::default()
        };
        let mut context_data: *mut SIGNER_CONTEXT = std::ptr::null_mut();
        let context = &raw mut context_data;
        // The two structs point at each other, so they are only accessed through raw pointers.
        let mut sip_data = AppxSipClientData { params: std::ptr::null_mut(), state: std::ptr::null_mut() };
        let mut params_data = SignerSignEx2Params {
            flags: 0,
            subject: &subject,
            cert: &signer,
            signature: &signature,
            provider: std::ptr::null(),
            timestamp_flags: 0,
            algorithm_oid: PCSTR::null(),
            timestamp_url: PCWSTR::null(),
            attributes: std::ptr::null(),
            sip_data: std::ptr::null(),
            context,
            crypto_policy: std::ptr::null(),
            reserved: std::ptr::null(),
        };
        let (sip, params) = (&raw mut sip_data, &raw mut params_data);
        (*sip).params = params;
        (*params).sip_data = sip.cast_const().cast();
        let result = SignerSignEx2(
            SIGNER_SIGN_FLAGS(0),
            &subject,
            &signer,
            &signature,
            None,
            None,
            PCSTR::null(),
            PCWSTR::null(),
            None,
            Some(sip.cast_const().cast()),
            context,
            None,
            None,
        );
        if !(*sip).state.is_null() {
            drop(IUnknown::from_raw((*sip).state));
        }
        if !(*context).is_null() {
            let _ = SignerFreeSignerContext(*context);
        }
        result
    }
}

/// Trusts the certificate (public part only) for packages on this computer.
fn trust(encoded: &[u8]) -> Result<()> {
    let store = Store::open(false)?;
    unsafe { CertAddEncodedCertificateToStore(Some(store.0), X509_ASN_ENCODING, encoded, CERT_STORE_ADD_REPLACE_EXISTING, None) }
}

/// The computer's "Trusted People" store (LocalMachine\TrustedPeople), where Windows looks for
/// publishers of packages installed outside the Microsoft Store.
struct Store(HCERTSTORE);

impl Store {
    fn open(read_only: bool) -> Result<Self> {
        let mut flags = CERT_SYSTEM_STORE_LOCAL_MACHINE;
        if read_only {
            flags |= CERT_STORE_READONLY_FLAG.0;
        }
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(flags),
                Some(w!("TrustedPeople").as_ptr().cast()),
            )?
        };
        Ok(Self(store))
    }

    /// The next Crest certificate after `previous` (which is freed); null if there is none.
    fn find(&self, previous: Option<*const CERT_CONTEXT>) -> *const CERT_CONTEXT {
        // Searched without "CN=", since this search uses the readable name.
        let name = HSTRING::from(SUBJECT.trim_start_matches("CN="));
        let found = unsafe {
            CertFindCertificateInStore(self.0, X509_ASN_ENCODING, 0, CERT_FIND_SUBJECT_STR, Some(name.as_ptr().cast()), previous)
        };
        // This search matches substrings: make sure the name is an exact match.
        if !found.is_null() && !exact_subject(found) {
            return self.find(Some(found));
        }
        found
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = unsafe { CertCloseStore(Some(self.0), 0) };
    }
}

fn exact_subject(cert: *const CERT_CONTEXT) -> bool {
    use windows::Win32::Security::Cryptography::{CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW};
    let mut buffer = [0u16; 128];
    let len = unsafe { CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buffer)) };
    let name = String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)]);
    name == SUBJECT.trim_start_matches("CN=")
}

/// Copy of a certificate context (for tests).
#[cfg(test)]
fn duplicate(cert: *const CERT_CONTEXT) -> *const CERT_CONTEXT {
    unsafe { windows::Win32::Security::Cryptography::CertDuplicateCertificateContext(Some(cert)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The publisher in the package manifest and in the crest.exe manifest must match exactly,
    /// otherwise Windows grants no identity.
    #[test]
    fn publishers_match() {
        let exe_manifest = include_str!("../../packaging/crest.exe.manifest");
        assert!(MANIFEST.contains(&format!("Publisher=\"{SUBJECT}\"")));
        assert!(exe_manifest.contains(&format!("publisher=\"{SUBJECT}\"")));
        assert!(MANIFEST.contains(&format!("Name=\"{PACKAGE_NAME}\"")));
        assert!(exe_manifest.contains(&format!("packageName=\"{PACKAGE_NAME}\"")));
    }

    #[test]
    fn manifest_follows_app_version() {
        let text = manifest("0.1.0");
        let identity = &text[text.find("<Identity").unwrap()..];
        assert!(identity[..identity.find("/>").unwrap()].contains("Version=\"0.1.0.0\""), "{identity}");
        // Only the package version changes, not other attributes that happen to be named Version.
        assert_eq!(text.matches("0.1.0.0").count(), 1);
    }

    #[test]
    fn file_uris() {
        assert_eq!(file_uri(Path::new(r"C:\Users\Jane Doe\AppData\Local\Programs\Crest")), "file:///C:/Users/Jane%20Doe/AppData/Local/Programs/Crest");
    }

    /// Builds and signs a package with a single-use key without changing anything on the
    /// computer (no certificate trusted, no key kept). Check the result with PowerShell: the
    /// signature is intact, only the publisher is not trusted yet.
    #[test]
    #[ignore]
    fn builds_and_signs_package() {
        let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
        let dir = std::env::temp_dir().join("crest-test-identity");
        std::fs::create_dir_all(&dir).unwrap();
        let package = dir.join(PACKAGE_FILE);
        // The package is only ever created as a new file (see `build_package`).
        let _ = std::fs::remove_file(&package);
        build_package(&package, &manifest("1.2.3")).unwrap();
        assert!(build_package(&package, &manifest("1.2.3")).is_err(), "an existing file must not be overwritten");
        let _ = std::fs::remove_file(&package);
        build_package(&package, &manifest("1.2.3")).unwrap();
        let key = Key::new().unwrap();
        let name = key.name.clone();
        let cert = Certificate::new(&key).unwrap();
        sign(&package, cert.0).unwrap();
        let copy = duplicate(cert.0);
        assert!(exact_subject(copy));
        let _ = unsafe { CertFreeCertificateContext(Some(copy)) };
        drop(cert);
        key.delete().unwrap();
        // The key is really gone from the Windows key store.
        unsafe {
            let mut provider = NCRYPT_PROV_HANDLE::default();
            NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0).unwrap();
            let mut handle = NCRYPT_KEY_HANDLE::default();
            let reopened = windows::Win32::Security::Cryptography::NCryptOpenKey(provider, &mut handle, &name, CERT_KEY_SPEC(0), NCRYPT_FLAGS(0));
            let _ = NCryptFreeObject(NCRYPT_HANDLE(provider.0));
            assert!(reopened.is_err(), "the key still exists");
        }
        println!("test package: {}", package.display());
    }
}
