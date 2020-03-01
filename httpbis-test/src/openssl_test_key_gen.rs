//! Utilities to generate keys for tests.
//!
//! This is copy-paste from tokio-tls.

use std::env;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::ptr;
use std::sync::Once;

pub struct ClientKeys {
    pub cert_der: Vec<u8>,
}

pub struct ServerKeys {
    pub pkcs12: Vec<u8>,
    pub pkcs12_password: String,

    pub pem: Vec<u8>,
}

pub struct Keys {
    pub client: ClientKeys,
    pub server: ServerKeys,
}

pub fn keys() -> &'static Keys {
    static INIT: Once = Once::new();
    static mut KEYS: *mut Keys = ptr::null_mut();

    INIT.call_once(|| {
        let path = t!(env::current_exe());
        let path = path.parent().unwrap();
        let keyfile = path.join("test.key");
        let certfile = path.join("test.crt");
        let config = path.join("openssl.config");

        File::create(&config)
            .unwrap()
            .write_all(
                b"\
                [req]\n\
                distinguished_name=dn\n\
                [dn]\n\
                CN=localhost\n\
                [ext]\n\
                basicConstraints=CA:FALSE,pathlen:0\n\
                subjectAltName = @alt_names\n\
                extendedKeyUsage=serverAuth,clientAuth\n\
                [alt_names]\n\
                DNS.1 = localhost\n\
            ",
            )
            .unwrap();

        let subj = "/C=US/ST=Denial/L=Sprintfield/O=Dis/CN=localhost";
        let output = t!(Command::new("openssl")
            .arg("req")
            .arg("-nodes")
            .arg("-x509")
            .arg("-newkey")
            .arg("rsa:2048")
            .arg("-config")
            .arg(&config)
            .arg("-extensions")
            .arg("ext")
            .arg("-subj")
            .arg(subj)
            .arg("-keyout")
            .arg(&keyfile)
            .arg("-out")
            .arg(&certfile)
            .arg("-days")
            .arg("1")
            .output());
        assert!(output.status.success());

        let crtout = t!(Command::new("openssl")
            .arg("x509")
            .arg("-outform")
            .arg("der")
            .arg("-in")
            .arg(&certfile)
            .output());
        assert!(crtout.status.success());

        let pkcs12out = t!(Command::new("openssl")
            .arg("pkcs12")
            .arg("-export")
            .arg("-nodes")
            .arg("-inkey")
            .arg(&keyfile)
            .arg("-in")
            .arg(&certfile)
            .arg("-password")
            .arg("pass:foobar")
            .output());
        assert!(pkcs12out.status.success());

        let pem = pkcs12_to_pem(&pkcs12out.stdout, "foobar");

        let keys = Box::new(Keys {
            client: ClientKeys {
                cert_der: crtout.stdout,
            },
            server: ServerKeys {
                pem,
                pkcs12: pkcs12out.stdout,
                pkcs12_password: "foobar".to_owned(),
            },
        });
        unsafe {
            KEYS = Box::into_raw(keys);
        }
    });
    unsafe { &*KEYS }
}

fn pkcs12_to_pem(pkcs12: &[u8], passin: &str) -> Vec<u8> {
    let command = t!(Command::new("openssl")
        .arg("pkcs12")
        .arg("-passin")
        .arg(&format!("pass:{}", passin))
        .arg("-nodes")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn());

    t!(command.stdin.unwrap().write_all(pkcs12));

    let mut pem = Vec::new();
    t!(command.stdout.unwrap().read_to_end(&mut pem));

    pem
}
