//! The curl config, its quoting, curl's output, and the encoders.

use super::*;

fn put() -> Request {
    let mut request = Request::new(
        Method::Put,
        "https://acme.atlassian.net/rest/api/3/issue/A-1?x=%22y%22".into(),
        vec![("Authorization".into(), "Basic s3cr\"et\\".into())],
    );
    request.json = Some("{\"summary\":\"two\\nlines \\\"quoted\\\"\"}".into());
    request
}

#[test]
fn quoting_escapes_what_curl_unescapes() {
    assert_eq!(quote("plain"), "\"plain\"");
    assert_eq!(quote("a\"b\\c\nd\te"), "\"a\\\"b\\\\c\\nd\\te\"");
}

#[test]
fn the_config_carries_the_whole_request_on_lines_of_their_own() {
    let config = curl_config(&put(), None);
    let lines: Vec<_> = config.lines().collect();
    assert!(lines.contains(&"request = \"PUT\""));
    assert!(lines.contains(&"url = \"https://acme.atlassian.net/rest/api/3/issue/A-1?x=%22y%22\""));
    assert!(lines.contains(&"header = \"Authorization: Basic s3cr\\\"et\\\\\""));
    assert!(lines.contains(&"header = \"Content-Type: application/json\""));
    assert!(lines.contains(&"globoff"), "JQL braces aren't globs");
    let data = lines.iter().find(|l| l.starts_with("data-binary")).unwrap();
    // A JSON body can't break out of its line or its quotes.
    assert_eq!(
        *data,
        "data-binary = \"{\\\"summary\\\":\\\"two\\\\nlines \\\\\\\"quoted\\\\\\\"\\\"}\""
    );
}

#[test]
fn debug_output_leaves_out_credentials() {
    let shown = format!("{:?}", put());
    assert!(!shown.contains("s3cr"), "{shown}");
    assert!(shown.contains("Authorization"));
}

#[test]
fn curls_output_gives_status_retry_after_and_body() {
    let r = parse_output(b"{\"a\":\n1}\n120\n429").unwrap();
    assert_eq!(r.status, 429);
    assert_eq!(r.retry_after, Some(Duration::from_secs(120)));
    assert_eq!(r.body, b"{\"a\":\n1}");
    let r = parse_output(b"\n\n204").unwrap();
    assert_eq!((r.status, r.body.len(), r.retry_after), (204, 0, None));
    assert!(parse_output(b"\n\n000").is_err());
    assert!(parse_output(b"garbage").is_err());
}

#[test]
fn encoders_match_their_standards() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"a@b.c:t0k3n"), "YUBiLmM6dDBrM24=");
    assert_eq!(encode_query("a b&c=\"d\"~é"), "a%20b%26c%3D%22d%22~%C3%A9");
}

/// A stand-in `curl` that keeps its config and answers like curl.
#[cfg(unix)]
#[test]
fn requests_reach_curl_only_through_its_stdin() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen");
    let args = dir.path().join("args");
    let script = dir.path().join("curl");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\ncat > '{}'\nprintf '{{\"ok\":true}}\\n\\n201'\n",
            args.display(),
            seen.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let answer = CurlHttp::with_program(script.to_string_lossy())
        .send(&put())
        .unwrap();
    assert_eq!(answer.status, 201);
    assert_eq!(answer.body, b"{\"ok\":true}");
    assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "--config -");
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap(),
        curl_config(&put(), None)
    );
}

#[test]
fn a_missing_curl_is_explained() {
    let error = CurlHttp::with_program("/no/such/curl")
        .send(&put())
        .unwrap_err();
    assert!(error.0.contains("curl isn't installed"), "{error}");
}

/// The real `curl` reads the config: an unreachable address fails as a connection
/// error, not as a malformed request. Skipped where curl isn't installed.
#[test]
fn the_system_curl_understands_the_config() {
    if std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let request = Request {
        url: "https://127.0.0.1:1/rest/api/3/myself".into(),
        ..put()
    };
    let error = CurlHttp::default().send(&request).unwrap_err();
    assert_eq!(error.0, "the site refused the connection");
}

#[test]
fn files_go_as_a_form_part_and_downloads_follow_redirects() {
    let mut request = Request::new(
        Method::Post,
        "https://acme.atlassian.net/rest/api/3/issue/A-1/attachments".into(),
        Vec::new(),
    );
    request.upload = Some(Upload {
        field: "file".into(),
        name: "plan \"v2\".png".into(),
        data: b"bytes".to_vec(),
    });
    let staged = std::path::Path::new("/tmp/x y/upload");
    let config = curl_config(&request, Some(staged));
    let form = config.lines().find(|l| l.starts_with("form = ")).unwrap();
    assert_eq!(
        form,
        r#"form = "file=@\"/tmp/x y/upload\";filename=\"plan \\\"v2\\\".png\"""#
    );
    assert!(!config.contains("location"));
    let mut download = Request::new(Method::Get, "https://x.y/f".into(), Vec::new());
    download.follow = true;
    assert!(
        curl_config(&download, None)
            .lines()
            .any(|l| l == "location")
    );
    // The staged file is the user's only, and goes once sent.
    let file = UploadFile::write(request.upload.as_ref().unwrap()).unwrap();
    assert_eq!(std::fs::read(&file.path).unwrap(), b"bytes");
    let dir = file.dir.clone();
    drop(file);
    assert!(!dir.exists());
    assert!(!format!("{request:?}").contains("bytes\""));
}
