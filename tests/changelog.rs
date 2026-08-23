#[test]
fn changelog_records_fail_closed_formatter_contract() {
    let changelog = include_str!("../CHANGELOG.md");

    for required_notice in [
        "`Result<String, FormatError>`",
        "byte-order mark",
        "custom `:::` containers",
        "`.mdx`",
    ] {
        assert!(
            changelog.contains(required_notice),
            "CHANGELOG.md must record {required_notice}"
        );
    }
}
