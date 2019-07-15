use std::ffi::{CString, CStr};
use libc::{size_t, c_int};
use std::os::raw::{c_char, c_void};
use log::Level;
use log::{info, trace, warn};

use std::fs::File;
use std::io::{Read, Write};
use utils::Result;
use utils::LAST_BACKTRACE;
use utils::LAST_ERROR;
use failure::Fail;

use tcx_bch::hd_mnemonic_keystore::HdMnemonicKeystore;
use tcx_bch::bitcoin_cash_transaction_signer::{BitcoinCashTransaction, Utxo};
use serde_json::Value;
use tcx_chain::{Metadata, Keystore, V3Keystore};
use std::path::Path;
use std::collections::HashMap;
use tcx_chain::signer::TransactionSinger;
use std::fs::{self, DirEntry};

use std::rc::Rc;
use std::cell::RefCell;
use core::borrow::{BorrowMut, Borrow};
use serde_json::map::Keys;
use std::sync::Mutex;
use crate::utils::set_panic_hook;

// #[link(name = "TrezorCrypto")]
// extern {
//     fn mnemonic_generate(strength: c_int, mnemonic: *mut c_char) -> c_int;
// }
//pub mod utils;

#[macro_use]
extern crate failure;

#[macro_use]
pub mod utils;

#[macro_use]
extern crate lazy_static;

static PASSWORD: &'static str = "Insecure Pa55w0rd";
static MNEMONIC: &'static str = "inject kidney empty canal shadow pact comfort wife crush horse wife sketch";
static ETHEREUM_PATH: &'static str = "m/44'/60'/0'/0/0";


lazy_static! {
    static ref KEYSTORE_MAP: Mutex<HashMap<String, Box<dyn Keystore>>> = {
        let mut m = Mutex::new(HashMap::new());

        let meta = Metadata::default();

        let keystore = HdMnemonicKeystore::new(meta, &PASSWORD, &MNEMONIC, &ETHEREUM_PATH).unwrap();
        m.lock().unwrap().insert("aaa".to_owned(), Box::new(keystore) as Box<Keystore>);
        m
    };

}

fn cache_keystore(keystore: Box<dyn Keystore>) {
    KEYSTORE_MAP.lock().unwrap().insert(keystore.get_id().to_owned(), keystore);
}

fn find_keystore(id: &str) -> Result<Box<dyn Keystore>> {
    let map = KEYSTORE_MAP.lock().unwrap();
    match map.get(&id.to_owned()) {
        Some(keystore) => Ok(keystore.clone_box()),
        _ => Err(format_err!("{}", "wallet_id_not_found"))
    }
}

fn find_keystore_by_address(address: &str) -> Option<Box<dyn Keystore>> {
    let map = KEYSTORE_MAP.lock().unwrap();
    let mut keystores: Vec<Box<dyn Keystore>> = map.iter()
        .filter(|(id, keystore)| keystore.get_address() == address)
        .map(|(id, keystore) | keystore.clone_box())
        .collect();

    keystores.reverse();
    keystores.pop()
}

#[no_mangle]
pub extern fn read_file(file_path: *const c_char) -> *const c_char {
    let c_str = unsafe { CStr::from_ptr(file_path) };
    let file_path = c_str.to_str().unwrap();
    // let filePath: String = env.get_string(filePath).expect("Couldn't get java string!").into();
    let mut file = File::open(file_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents);

    CString::new(contents).unwrap().into_raw()
}

#[no_mangle]
pub extern fn free_string(s: *mut c_char) {
    unsafe {
        if s.is_null() { return; }
        CString::from_raw(s)
    };
}

#[no_mangle]
pub extern fn free_const_string(s: *const c_char) {
    unsafe {
        if s.is_null() { return; }
        CStr::from_ptr(s)
    };
}

#[no_mangle]
pub unsafe extern "C" fn read_file_error() -> *const c_char {
    crate::utils::landingpad(||
        {
            Err(format_err!("{}", "read file error"))
        })
}

#[no_mangle]
pub unsafe extern "C" fn import_bch_wallet_from_mnemonic(mnemonic: *const c_char, password: *const c_char) -> *const c_char {
    let mnemonic_c_str = unsafe { CStr::from_ptr(mnemonic) };
    let mnemonic = mnemonic_c_str.to_str().unwrap();
    let password_c_str = unsafe { CStr::from_ptr(password) };
    let password = password_c_str.to_str().unwrap();
    let meta = Metadata::default();
    let keystore = HdMnemonicKeystore::new(meta, password, mnemonic, "m/44'/0'/0'").unwrap();
    let json = keystore.export_json();
    CString::new(json).unwrap().into_raw()
}

fn parse_arguments(json_str: *const c_char) -> Value {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();
    serde_json::from_str(json_str).unwrap()
}


#[no_mangle]
pub unsafe extern "C" fn scan_wallets(json_str: *const c_char) {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();
    let v: Value = serde_json::from_str(json_str).unwrap();
    set_panic_hook();
    _scan_wallets(v);
        ()
}

fn _scan_wallets(v: Value) -> Result<()> {
    let file_dir = v["fileDir"].as_str().unwrap();
    info!("scan file {}", file_dir);
    let p = Path::new(file_dir);
    let walk_dir = std::fs::read_dir(p).unwrap();
    info!("walk dir {:?}", walk_dir);
    for entry in walk_dir {
        let entry = entry.unwrap();
        let fp = entry.path();
        let mut f = File::open(fp).unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents);
        let v: Value = serde_json::from_str(&contents).unwrap();

        let version = v["version"].as_i64().unwrap();
        if v["metadata"]["chainType"].as_str().unwrap().to_uppercase() != "BCH" {
            continue;
        }
        let keystore: Result<Box<dyn Keystore>> = match version {
            44 => {
                let keystore: HdMnemonicKeystore = serde_json::from_str(&contents).unwrap();
                Ok(Box::new(keystore) as Box<dyn Keystore>)
            }
            3 => {
                let keystore: V3Keystore = serde_json::from_str(&contents).unwrap();
                Ok(Box::new(keystore) as Box<dyn Keystore>)
            }
            _ => {
                Err(format_err!("{}", "unknown_version"))
            }
        };
        cache_keystore(keystore?);
    }
    Ok(())
}

fn _find_wallet_by_mnemonic(json_str: &str) -> Result<String> {
    let v: Value = serde_json::from_str(json_str).unwrap();
    let mnemonic = v["mnemonic"].as_str().unwrap();
    let path = v["path"].as_str().unwrap();
    let network = v["network"].as_str().unwrap();
    // todo: provider support

    let address = HdMnemonicKeystore::address_from_mnemonic(mnemonic, path, network)?;
    let keystore = find_keystore_by_address(&address);
    if let Some(ks) = keystore {
        Ok(ks.export_json())
    } else {
        Ok("{}".to_owned())
    }
}

#[no_mangle]
pub unsafe extern "C" fn find_wallet_by_mnemonic(json_str: *const c_char) -> *const c_char {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();

    let json = crate::utils::landingpad(|| _find_wallet_by_mnemonic(json_str));
    CString::new(json).unwrap().into_raw()
}

fn _import_wallet_from_mnemonic(json_str: &str) -> Result<String> {
    let v: Value = serde_json::from_str(json_str).unwrap();

    let mut meta: Metadata = serde_json::from_value(v.clone())?;
    let password = v["password"].as_str().unwrap();
    let mnemonic = v["mnemonic"].as_str().unwrap();
    let path = v["path"].as_str().unwrap();
    let overwrite = v["overwrite"].as_bool().unwrap();
    let file_dir = v["fileDir"].as_str().unwrap();

    let mut keystore = HdMnemonicKeystore::new(meta, password, mnemonic, path)?;

    let exist_ks_opt = find_keystore_by_address(&keystore.address);
    if exist_ks_opt.is_some() {
        if !overwrite {
            return Err(format_err!("{}", "wallet_exists"))
        } else {
            keystore.id = exist_ks_opt.unwrap().get_id();
        }
    }

    let json = keystore.export_json();

    let ks_path = format!("{}{}.json", file_dir, keystore.id);
    let path = Path::new(&ks_path);
    let mut file = File::create(path).unwrap();
    file.write_all(&json.as_bytes());

    let keystore_box :Box<dyn Keystore> = Box::new(keystore);
    cache_keystore(keystore_box);
    Ok(json)
}


#[no_mangle]
pub unsafe extern "C" fn import_wallet_from_mnemonic(json_str: *const c_char) -> *const c_char {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();
    let json = crate::utils::landingpad(|| _import_wallet_from_mnemonic(json_str));
    CString::new(json).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn import_wallet_from_private_key(json_str: *const c_char) -> *const c_char {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();
    let v: Value = serde_json::from_str(json_str).unwrap();

    let mut meta: Metadata = serde_json::from_str(json_str).unwrap();
    let password = v["password"].as_str().unwrap();
    let private_key = v["privateKey"].as_str().unwrap();
    let overwrite = v["overwrite"].as_bool().unwrap();
    let file_dir = v["fileDir"].as_str().unwrap();

    let keystore = V3Keystore::new(meta, password, private_key).unwrap();
    let json = keystore.export_json();

    let ks_path = format!("{}{}.json", file_dir, keystore.id);
    let path = Path::new(&ks_path);
    if path.exists() && !overwrite {
        // throw error
    }
    let mut file = File::create(path).unwrap();
    file.write_all(&json.as_bytes());
    let keystore_box: Box<dyn Keystore> = Box::new(keystore);
    cache_keystore(keystore_box);

    CString::new(json).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn sign_transaction(json_str: *const c_char) -> *const c_char {
    let json_c_str = unsafe { CStr::from_ptr(json_str) };
    let json_str = json_c_str.to_str().unwrap();

    let json = crate::utils::landingpad(|| _sign_transaction(json_str));
    CString::new(json).unwrap().into_raw()
}

fn _sign_transaction(json_str: &str) -> Result<String> {
    let v: Value = serde_json::from_str(json_str).unwrap();
    let w_id = v["id"].as_str().unwrap();
    let unspents: Vec<Utxo> = serde_json::from_value(v["outputs"].clone()).unwrap();
    let internal_used = v["internalUsed"].as_i64().unwrap();
    let change_idx = internal_used + 1;
    let to = v["to"].as_str().unwrap();
    let amount = v["amount"].as_str().unwrap().parse::<i64>().unwrap();
    let fee = v["fee"].as_str().unwrap().parse::<i64>().unwrap();
    let password = v["password"].as_str().unwrap();
    let chain_id = v["chainId"].as_str().unwrap();

    let bch_tran = BitcoinCashTransaction {
        to: to.to_owned(),
        amount,
        unspents,
        memo: "".to_string(),
        fee,
        change_idx: change_idx as u32,
    };

    let keystore = find_keystore(w_id)?;
    let ret = bch_tran.sign_transaction(chain_id, password, keystore.as_ref())?;
    Ok(serde_json::to_string(&ret)?)
}

//fn _find_wallet_by_mnemonic(json_str: &str) -> Result<String> {
//    let v: Value = serde_json::from_str(json_str)?;
//    let mnemonic = v["mnemonic"].as_str()?;
//    let path = v["path"].as_str()?;
//    let network = v["network"].as_str()?;
//    let chain_type = v["chainType"].as_str()?;
//
//
//
//
//
//}


#[no_mangle]
pub unsafe extern "C" fn clear_err() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
    LAST_BACKTRACE.with(|e| {
        *e.borrow_mut() = None;
    });
}

#[no_mangle]
pub unsafe extern "C" fn get_last_err_message() -> *const c_char {
    use std::fmt::Write;
    use std::error::Error;
    LAST_ERROR.with(|e| {
        if let Some(ref err) = *e.borrow() {
            let mut msg = err.to_string();
            // todo: follow cause
//            let mut cause = err.cause();
//            while let Some(the_cause) = cause {
//                write!(&mut msg, "\n  caused by: {}", the_cause).ok();
//                cause = &the_cause.cause();
//            }
            CString::new(msg).unwrap().into_raw()
        } else {
            CString::new("").unwrap().into_raw()
        }
    })
}


#[cfg(test)]
mod tests {
    use crate::import_wallet_from_mnemonic;
    use std::ffi::{CString, CStr};

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    //    #[test]
//    unsafe fn import_wallet() {
//        let data = r#"
//        {
//            "password": "PASSWORD",
//            "mnemonic": "inject kidney empty canal shadow pact comfort wife crush horse wife sketch",
//            "path": "m/44'/145'/0'",
//            "overwrite": false,
//            "name": "bch-ios",
//            "passwordHint": "",
//            "chainType": "BCH",
//            "network": "MAINNET",
//            "fileDir": "/tmp/imtoken/wallets"
//
//        }"#;
//        let json_str = CString::new(data).unwrap().into_raw();
//        let ret = unsafe { import_wallet_from_mnemonic(json_str)};
//        assert_eq!("", CStr::from_ptr(ret).to_str().unwrap());
//    }
    #[test]
    fn path() {
        let file_dir = "/Users/xyz/Library/Developer/CoreSimulator/Devices/1C6326AE-C550-43D5-A1A7-CF791B4A04CA/data/Containers/Data/Application/BC076852-DF07-42EA-82B1-2FA8C5CEE9EE/Documents/wallets/";
        let id = "ec9298f7-7f2b-4483-90af-cc440a411d82";
        let ks_path = format!("{}{}.json", file_dir, id);
        assert_eq!("", ks_path);
    }
}