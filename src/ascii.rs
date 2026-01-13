#![allow(dead_code)]
/// ASCII logo helper (single variant).
///
/// The ASCII art is provided as a public constant `ASCII_LOGO` (slice of lines)
/// and a small helper `print_ascii_logo()` to print it to stdout.
///
/// Note: the lines are raw-string literals so backslashes are preserved exactly.
pub const ASCII_LOGO: &[&str] = &[
    r#"                              $$\ $$\                       $$\ $$\ "#,
    r#"                              \__|$$ |                      $$ |\__|"#,
    r#"      $$\  $$$$$$\   $$$$$$\  $$\ $$ |  $$\        $$$$$$$\ $$ |$$\ "#,
    r#"      \__|$$  __$$\ $$  __$$\ $$ |$$ | $$  |      $$  _____|$$ |$$ |"#,
    r#"      $$\ $$ /  $$ |$$ |  \__|$$ |$$$$$$  /       $$ /      $$ |$$ |"#,
    r#"      $$ |$$ |  $$ |$$ |      $$ |$$  _$$<        $$ |      $$ |$$ |"#,
    r#"      $$ |\$$$$$$  |$$ |      $$ |$$ | \$$\       \$$$$$$$\ $$ |$$ |"#,
    r#"      $$ | \______/ \__|      \__|\__|  \__|       \_______|\__|\__|"#,
    r#"$$\   $$ |                                                           "#,
    r#"\$$$$$$  |                                                           "#,
    r#" \______/                                                            "#,
];

/// Print the ascii logo to stdout.
pub fn print_ascii_logo() {
    for line in ASCII_LOGO.iter() {
        println!("{}", line);
    }
}
