use regex::Regex;

fn main() {
    cynic_codegen::register_schema("saleor")
        .from_sdl_file("schemas/saleor.graphql")
        .unwrap()
        .as_default()
        .unwrap();
}