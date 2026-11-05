use super::{Endpoint, quote};

#[test]
fn host_and_path_validation_reject_option_or_command_injection() {
    for host in [
        "",
        "-oProxyCommand=bad",
        "user@host;bad",
        "user@host\n",
        "user@@host",
        "@host",
        "user@host/path",
    ] {
        let endpoint = Endpoint {
            host: host.into(),
            root: "/neutral/knowledge".into(),
            executable: "/neutral/dots".into(),
        };
        assert!(endpoint.validate().is_err(), "{host:?}");
    }
    for host in [
        "replica",
        "user_name@replica.tail.invalid",
        "user@100.64.0.1",
    ] {
        let endpoint = Endpoint {
            host: host.into(),
            root: "/neutral/knowledge".into(),
            executable: "/neutral/dots".into(),
        };
        assert!(endpoint.validate().is_ok(), "{host}");
    }
    let endpoint = Endpoint {
        host: "replica".into(),
        root: "relative".into(),
        executable: "/neutral/dots".into(),
    };
    assert!(endpoint.validate().is_err());
}

#[test]
fn native_ssh_command_quotes_one_path_without_interpreting_its_bytes() {
    assert_eq!(
        quote("/neutral/path with space"),
        "'/neutral/path with space'"
    );
    assert_eq!(quote("/neutral/it's-here"), "'/neutral/it'\\''s-here'");
}
