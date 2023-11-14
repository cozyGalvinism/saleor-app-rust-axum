use regex::Regex;

fn main() {
    // workaround until cynic-cli generates schemas with single line deprecated reasons
    // see https://github.com/obmarg/cynic/issues/790
    let deprecation_regex = Regex::new(r#"(@deprecated\(reason:\s*)"([\s\S]*?)"\)"#).unwrap();
    let multiline_regex = Regex::new(r"[\r\n]+").unwrap();

    let sdl = std::fs::read_to_string("schemas/saleor.graphql").unwrap();
    let sdl = deprecation_regex.replace_all(&sdl, |caps: &regex::Captures| {
        let reason = &caps[2];
        let reason = multiline_regex.replace_all(reason, " ");
        let reason = reason.trim();
        format!(r#"{}"{}")"#, &caps[1], reason)
    });

    cynic_codegen::register_schema("saleor")
        .from_sdl(&sdl)
        .unwrap()
        .as_default()
        .unwrap();
}