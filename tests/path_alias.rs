mod fixtures;
mod utils;

use fixtures::{server, Error, TestServer};
use rstest::rstest;

#[cfg(unix)]
use std::os::unix::fs::{symlink as symlink_dir, symlink as symlink_file};
#[cfg(windows)]
use std::os::windows::fs::{symlink_dir, symlink_file};

fn readonly_aliases(server: &TestServer) -> Result<(), Error> {
    let root = server.path();
    std::fs::create_dir_all(root.join("protected"))?;
    std::fs::create_dir_all(root.join("public"))?;
    std::fs::write(root.join("protected/file.txt"), "unchanged")?;
    symlink_dir(root.join("protected"), root.join("shortcut"))?;
    symlink_dir(root.join("protected"), root.join("public/shortcut"))?;
    symlink_file(root.join("protected/file.txt"), root.join("shortcut.txt"))?;
    Ok(())
}

#[rstest]
#[case::put("PUT")]
#[case::patch("PATCH")]
#[case::delete("DELETE")]
#[case::relocate("MOVE")]
#[case::copy("COPY")]
fn readonly_symlink_alias_source(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
    #[case] method: &str,
) -> Result<(), Error> {
    readonly_aliases(&server)?;
    for path in [
        "protected/file.txt",
        "shortcut/file.txt",
        "public/shortcut/file.txt",
        "shortcut.txt",
    ] {
        let response = reqwest::blocking::Client::new()
            .request(
                reqwest::Method::from_bytes(method.as_bytes())?,
                format!("{}{path}", server.url()),
            )
            .basic_auth("user", Some("pass"))
            .header("Destination", format!("{}moved.txt", server.url()))
            .header("X-Update-Range", "append")
            .body("replacement")
            .send()?;
        assert_eq!(response.status(), 403, "{method} {path}");
        assert_eq!(
            std::fs::read_to_string(server.path().join("protected/file.txt"))?,
            "unchanged"
        );
        assert!(!server.path().join("moved.txt").exists());
    }
    Ok(())
}

#[rstest]
#[case::put("PUT", "new/deep/file.txt")]
#[case::mkdir("MKCOL", "new/deep")]
fn readonly_symlink_alias_missing_target(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
    #[case] method: &str,
    #[case] suffix: &str,
) -> Result<(), Error> {
    readonly_aliases(&server)?;
    for alias in ["shortcut", "public/shortcut"] {
        let response = reqwest::blocking::Client::new()
            .request(
                reqwest::Method::from_bytes(method.as_bytes())?,
                format!("{}{alias}/{suffix}", server.url()),
            )
            .basic_auth("user", Some("pass"))
            .body("new content")
            .send()?;
        assert_eq!(response.status(), 403, "{method} {alias}/{suffix}");
        assert!(!server.path().join("protected/new").exists());
    }
    Ok(())
}

#[rstest]
#[case::relocate("MOVE")]
#[case::copy("COPY")]
fn readonly_symlink_alias_destination(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
    #[case] method: &str,
) -> Result<(), Error> {
    readonly_aliases(&server)?;
    std::fs::write(server.path().join("source.txt"), "source")?;
    for alias in ["shortcut", "public/shortcut"] {
        let response = reqwest::blocking::Client::new()
            .request(
                reqwest::Method::from_bytes(method.as_bytes())?,
                format!("{}source.txt", server.url()),
            )
            .basic_auth("user", Some("pass"))
            .header(
                "Destination",
                format!("{}{alias}/new/file.txt", server.url()),
            )
            .send()?;
        assert_eq!(response.status(), 403, "{method} destination {alias}");
        assert_eq!(
            std::fs::read_to_string(server.path().join("source.txt"))?,
            "source"
        );
        assert!(!server.path().join("protected/new").exists());
    }
    Ok(())
}

#[rstest]
fn readonly_symlink_alias_across_rule_branches(
    #[with(&["--auth", "user:pass@/:rw,/nested/protected:ro", "--allow-upload", "--allow-delete", "--allow-symlink"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("nested/protected"))?;
    symlink_dir(
        server.path().join("nested/protected"),
        server.path().join("shortcut"),
    )?;
    let response = fetch!(b"PUT", format!("{}shortcut/new.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("new content")
        .send()?;
    assert_eq!(response.status(), 403);
    assert!(!server.path().join("nested/protected/new.txt").exists());
    Ok(())
}

#[rstest]
fn writable_symlink_alias_stays_readable_and_writable(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("writable"))?;
    symlink_dir(
        server.path().join("writable"),
        server.path().join("shortcut"),
    )?;
    let response = fetch!(b"PUT", format!("{}shortcut/new.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("writable content")
        .send()?;
    assert_eq!(response.status(), 201);
    assert_eq!(
        std::fs::read_to_string(server.path().join("writable/new.txt"))?,
        "writable content"
    );
    let response = fetch!(b"GET", format!("{}shortcut/new.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .send()?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text()?, "writable content");
    Ok(())
}

#[rstest]
fn explicit_readonly_symlink_rule_stays_readonly(
    #[with(&["--auth", "user:pass@/:rw,/shortcut:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("writable"))?;
    symlink_dir(
        server.path().join("writable"),
        server.path().join("shortcut"),
    )?;
    let response = fetch!(b"PUT", format!("{}shortcut/new.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("new content")
        .send()?;
    assert_eq!(response.status(), 403);
    assert!(!server.path().join("writable/new.txt").exists());
    Ok(())
}

#[rstest]
fn readonly_symlink_alias_remains_readable(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    readonly_aliases(&server)?;
    let response = fetch!(b"GET", format!("{}public/shortcut/file.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .send()?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text()?, "unchanged");
    Ok(())
}

#[rstest]
fn readonly_dangling_file_alias_cannot_create_its_target(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("protected"))?;
    symlink_file(
        server.path().join("protected/new.txt"),
        server.path().join("shortcut.txt"),
    )?;
    let response = fetch!(b"PUT", format!("{}shortcut.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("new content")
        .send()?;
    assert_eq!(response.status(), 403);
    assert!(!server.path().join("protected/new.txt").exists());
    Ok(())
}

#[rstest]
fn readonly_symlink_alias_form_upload_respects_full_policy(
    #[with(&["--auth", "user:pass@/:rw,/protected:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    readonly_aliases(&server)?;
    symlink_file(
        server.path().join("protected/file.txt"),
        server.path().join("public/shortcut.txt"),
    )?;
    let form = reqwest::blocking::multipart::Form::new().part(
        "file",
        reqwest::blocking::multipart::Part::text("replacement").file_name("shortcut.txt"),
    );
    let response = fetch!(b"POST", format!("{}public/?noscript", server.url()))
        .basic_auth("user", Some("pass"))
        .multipart(form)
        .send()?;
    assert_eq!(response.status(), 403);
    assert_eq!(
        std::fs::read_to_string(server.path().join("protected/file.txt"))?,
        "unchanged"
    );
    Ok(())
}

#[rstest]
fn symlink_grant_does_not_grant_access_to_an_unconfigured_target(
    #[with(&["--auth", "user:pass@/shortcut:rw", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("unconfigured"))?;
    std::fs::write(server.path().join("unconfigured/file.txt"), "unchanged")?;
    symlink_dir(
        server.path().join("unconfigured"),
        server.path().join("shortcut"),
    )?;
    for method in ["GET", "PUT"] {
        let response = reqwest::blocking::Client::new()
            .request(
                reqwest::Method::from_bytes(method.as_bytes())?,
                format!("{}shortcut/file.txt", server.url()),
            )
            .basic_auth("user", Some("pass"))
            .body("replacement")
            .send()?;
        assert_eq!(
            response.status(),
            403,
            "{method} through explicit alias grant"
        );
        assert_eq!(
            std::fs::read_to_string(server.path().join("unconfigured/file.txt"))?,
            "unchanged"
        );
    }
    Ok(())
}

#[rstest]
fn readonly_descendants_of_symlink_destination_remain_protected(
    #[with(&["--auth", "user:pass@/:rw,/writable/target/keep:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("writable"))?;
    std::fs::create_dir_all(server.path().join("source/keep"))?;
    std::fs::write(server.path().join("source/keep/file.txt"), "unchanged")?;
    symlink_dir(
        server.path().join("writable"),
        server.path().join("shortcut"),
    )?;
    let response = fetch!(b"MOVE", format!("{}source", server.url()))
        .basic_auth("user", Some("pass"))
        .header("Destination", format!("{}shortcut/target", server.url()))
        .send()?;
    assert_eq!(response.status(), 403);
    assert_eq!(
        std::fs::read_to_string(server.path().join("source/keep/file.txt"))?,
        "unchanged"
    );
    assert!(!server.path().join("writable/target").exists());
    Ok(())
}

#[rstest]
fn explicit_symlink_and_target_grants_preserve_writable_exceptions(
    #[with(&["--auth", "user:pass@/:ro,/shortcut:rw,/writable:rw,/writable/locked.txt:ro", "--allow-upload", "--allow-delete"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("writable"))?;
    std::fs::write(server.path().join("writable/locked.txt"), "unchanged")?;
    symlink_dir(
        server.path().join("writable"),
        server.path().join("shortcut"),
    )?;
    let response = fetch!(b"PUT", format!("{}shortcut/open.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("writable content")
        .send()?;
    assert_eq!(response.status(), 201);
    assert_eq!(
        std::fs::read_to_string(server.path().join("writable/open.txt"))?,
        "writable content"
    );
    let response = fetch!(b"PUT", format!("{}shortcut/locked.txt", server.url()))
        .basic_auth("user", Some("pass"))
        .body("replacement")
        .send()?;
    assert_eq!(response.status(), 403);
    assert_eq!(
        std::fs::read_to_string(server.path().join("writable/locked.txt"))?,
        "unchanged"
    );
    Ok(())
}

fn unconfigured_child_alias(server: &TestServer) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("public"))?;
    std::fs::create_dir_all(server.path().join("unconfigured"))?;
    std::fs::write(server.path().join("public/visible.txt"), "visible")?;
    std::fs::write(server.path().join("unconfigured/secret.txt"), "private")?;
    symlink_dir(
        server.path().join("unconfigured"),
        server.path().join("public/shortcut"),
    )?;
    Ok(())
}

#[rstest]
fn archive_excludes_unconfigured_physical_targets_of_child_aliases(
    #[with(&["--auth", "user:pass@/public:rw", "--allow-archive"])] server: TestServer,
) -> Result<(), Error> {
    unconfigured_child_alias(&server)?;
    let response = fetch!(b"GET", format!("{}public/?zip", server.url()))
        .basic_auth("user", Some("pass"))
        .send()?;
    assert_eq!(response.status(), 200);
    let archive = zip::ZipArchive::new(std::io::Cursor::new(response.bytes()?))?;
    let names = archive.file_names().collect::<Result<Vec<_>, _>>()?;
    assert!(names.iter().any(|name| name == "visible.txt"));
    assert!(!names.iter().any(|name| name.starts_with("shortcut/")));
    Ok(())
}

#[rstest]
fn search_excludes_unconfigured_physical_targets_of_child_aliases(
    #[with(&["--auth", "user:pass@/public:rw", "--allow-search"])] server: TestServer,
) -> Result<(), Error> {
    unconfigured_child_alias(&server)?;
    let response = fetch!(b"GET", format!("{}public/?q=txt", server.url()))
        .basic_auth("user", Some("pass"))
        .send()?;
    assert_eq!(response.status(), 200);
    let paths = utils::retrieve_index_paths(&response.text()?);
    assert!(paths.contains("visible.txt"));
    assert!(!paths.iter().any(|name| name.starts_with("shortcut/")));
    Ok(())
}

#[rstest]
fn archive_keeps_readable_descendants_of_an_index_only_alias_target(
    #[with(&["--auth", "user:pass@/public:rw,/physical/open:ro", "--allow-archive"])]
    server: TestServer,
) -> Result<(), Error> {
    std::fs::create_dir_all(server.path().join("public"))?;
    std::fs::create_dir_all(server.path().join("physical/open"))?;
    std::fs::write(server.path().join("physical/open/allowed.txt"), "readable")?;
    std::fs::write(server.path().join("physical/blocked.txt"), "private")?;
    symlink_dir(
        server.path().join("physical"),
        server.path().join("public/shortcut"),
    )?;
    let response = fetch!(b"GET", format!("{}public/?zip", server.url()))
        .basic_auth("user", Some("pass"))
        .send()?;
    assert_eq!(response.status(), 200);
    let archive = zip::ZipArchive::new(std::io::Cursor::new(response.bytes()?))?;
    let names = archive.file_names().collect::<Result<Vec<_>, _>>()?;
    assert!(names.iter().any(|name| name == "shortcut/open/allowed.txt"));
    assert!(!names.iter().any(|name| name == "shortcut/blocked.txt"));
    Ok(())
}
