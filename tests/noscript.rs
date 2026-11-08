mod fixtures;
mod utils;

use fixtures::{server, Error, TestServer};
use reqwest::blocking::multipart::{Form, Part};
use rstest::rstest;

/// Client that does not follow the 303 redirect so we can assert on it.
fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[rstest]
fn noscript_page_has_forms(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let text = reqwest::blocking::get(format!("{}?noscript", server.url()))?.text()?;
    assert!(text.contains("Upload"));
    assert!(text.contains("New folder"));
    assert!(text.contains("Del"));
    assert!(text.contains(r#"name="mkdir""#));
    assert!(text.contains(r#"name="delete""#));
    Ok(())
}

#[rstest]
fn noscript_page_no_forms(server: TestServer) -> Result<(), Error> {
    let text = reqwest::blocking::get(format!("{}?noscript", server.url()))?.text()?;
    assert!(!text.contains("Upload"));
    assert!(!text.contains("New folder"));
    assert!(!text.contains("Del"));
    Ok(())
}

#[cfg(not(windows))]
#[rstest]
#[case::quoted_name("a\"&<'quoted.txt")]
#[case::attribute_injection("a\" autofocus=\"\" onfocus=\"alert(1)&<'.txt")]
fn delete_form_filename_round_trip(
    #[with(&["-A"])] server: TestServer,
    #[case] filename: &str,
) -> Result<(), Error> {
    let upload = client()
        .put(format!("{}{}", server.url(), utils::encode_uri(filename)))
        .body("uploaded")
        .send()?;
    assert_eq!(upload.status(), 201);
    let page = client()
        .get(format!("{}?noscript", server.url()))
        .send()?
        .text()?;
    let mut found = false;
    for line in page.lines().filter(|line| {
        line.trim_start()
            .starts_with("<input type=\"hidden\" name=\"name\"")
    }) {
        // These input tags use XML-compatible attributes. Close the void tag
        // to parse its attribute values and detect injected extra attributes.
        let fragment = format!("{}/>", line.trim().trim_end_matches('>'));
        for event in xml::reader::EventReader::from_str(&fragment) {
            if let xml::reader::XmlEvent::StartElement { attributes, .. } = event? {
                assert_eq!(attributes.len(), 3, "unexpected attributes: {line}");
                let value = attributes
                    .iter()
                    .find(|attribute| attribute.name.local_name == "value")
                    .unwrap();
                if value.value == filename {
                    found = true;
                    let deleted = client()
                        .post(format!("{}?noscript", server.url()))
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(format!(
                            "delete=1&name={}",
                            urlencoding::encode(&value.value)
                        ))
                        .send()?;
                    assert_eq!(deleted.status(), 303);
                    assert!(!server.path().join(filename).exists());
                    assert!(server.path().join("test.txt").exists());
                }
            }
        }
    }
    assert!(found, "filename did not round-trip through the delete form");
    Ok(())
}

#[rstest]
fn upload_file(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let form = Form::new().part(
        "file",
        Part::bytes(b"hello".to_vec()).file_name("upload.txt"),
    );
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 303);
    assert_eq!(
        std::fs::read_to_string(server.path().join("upload.txt"))?,
        "hello"
    );
    Ok(())
}

#[rstest]
fn upload_empty_file(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let form = Form::new().part("file", Part::bytes(Vec::new()).file_name("empty.txt"));
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 303);
    assert_eq!(std::fs::metadata(server.path().join("empty.txt"))?.len(), 0);
    Ok(())
}

#[rstest]
fn upload_multiple(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let form = Form::new()
        .part("file", Part::bytes(b"a".to_vec()).file_name("a.txt"))
        .part("file", Part::bytes(b"b".to_vec()).file_name("b.txt"));
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 303);
    assert_eq!(std::fs::read_to_string(server.path().join("a.txt"))?, "a");
    assert_eq!(std::fs::read_to_string(server.path().join("b.txt"))?, "b");
    Ok(())
}

#[rstest]
fn upload_traversal_name(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let form = Form::new().part("file", Part::bytes(b"x".to_vec()).file_name("../evil.txt"));
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 303);
    // sanitized to the basename, stored inside the served root
    assert_eq!(
        std::fs::read_to_string(server.path().join("evil.txt"))?,
        "x"
    );
    assert!(!server.path().parent().unwrap().join("evil.txt").exists());
    Ok(())
}

#[rstest]
fn upload_forbidden(server: TestServer) -> Result<(), Error> {
    let form = Form::new().part("file", Part::bytes(b"x".to_vec()).file_name("no.txt"));
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 403);
    Ok(())
}

#[rstest]
fn post_to_file_forbidden(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}test.txt?noscript", server.url()))
        .send()?;
    assert_eq!(resp.status(), 403);
    Ok(())
}

#[rstest]
fn upload_missing_boundary(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .header("content-type", "multipart/form-data")
        .body("garbage")
        .send()?;
    assert_eq!(resp.status(), 400);
    Ok(())
}

#[rstest]
fn mkdir(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("mkdir=1&name=newdir")
        .send()?;
    assert_eq!(resp.status(), 303);
    assert!(server.path().join("newdir").is_dir());
    Ok(())
}

#[rstest]
fn mkdir_nested_dir(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}dir1/?noscript", server.url()))
        .body("mkdir=1&name=sub")
        .send()?;
    assert_eq!(resp.status(), 303);
    assert!(server.path().join("dir1").join("sub").is_dir());
    Ok(())
}

#[rstest]
fn mkdir_traversal_name(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("mkdir=1&name=%2E%2E%2Fevil")
        .send()?;
    assert_eq!(resp.status(), 400);
    assert!(!server.path().parent().unwrap().join("evil").exists());
    assert!(!server.path().join("evil").exists());
    Ok(())
}

#[rstest]
fn mkdir_forbidden(server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("mkdir=1&name=newdir")
        .send()?;
    assert_eq!(resp.status(), 403);
    assert!(!server.path().join("newdir").exists());
    Ok(())
}

#[rstest]
fn delete_file(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    assert!(server.path().join("test.txt").exists());
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("delete=1&name=test.txt")
        .send()?;
    assert_eq!(resp.status(), 303);
    assert!(!server.path().join("test.txt").exists());
    Ok(())
}

#[rstest]
fn delete_not_exist(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("delete=1&name=nope.txt")
        .send()?;
    assert_eq!(resp.status(), 404);
    Ok(())
}

#[rstest]
fn delete_forbidden(server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .body("delete=1&name=test.txt")
        .send()?;
    assert_eq!(resp.status(), 403);
    assert!(server.path().join("test.txt").exists());
    Ok(())
}

#[rstest]
fn post_empty_form(#[with(&["-A"])] server: TestServer) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("")
        .send()?;
    assert_eq!(resp.status(), 400);
    Ok(())
}

#[rstest]
fn upload_overwrite_needs_delete(
    #[with(&["--allow-upload"])] server: TestServer,
) -> Result<(), Error> {
    let form = Form::new().part("file", Part::bytes(b"new".to_vec()).file_name("test.txt"));
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .multipart(form)
        .send()?;
    assert_eq!(resp.status(), 403);
    assert_eq!(
        resp.headers().get("x-dufs-error-code").unwrap(),
        "delete-disabled"
    );
    assert_eq!(
        std::fs::read_to_string(server.path().join("test.txt"))?,
        "This is test.txt"
    );
    Ok(())
}

#[rstest]
fn delete_respects_readonly_child(
    #[with(&["--auth", "user:pass@/:rw,/dir1:ro", "-A"])] server: TestServer,
) -> Result<(), Error> {
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .basic_auth("user", Some("pass"))
        .body("delete=1&name=dir1")
        .send()?;
    assert_eq!(resp.status(), 403);
    assert!(server.path().join("dir1/test.txt").exists());
    Ok(())
}

#[rstest]
fn delete_dir_with_readonly_descendant(
    #[with(&["--auth", "user:pass@/:rw,/dir1/sub:ro", "-A"])] server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("dir1/sub"))?;
    let resp = client()
        .post(format!("{}?noscript", server.url()))
        .basic_auth("user", Some("pass"))
        .body("delete=1&name=dir1")
        .send()?;
    assert_eq!(resp.status(), 403);
    assert_eq!(
        resp.headers().get("x-dufs-error-code").unwrap(),
        "readonly-descendant"
    );
    assert!(server.path().join("dir1/sub").exists());
    Ok(())
}
