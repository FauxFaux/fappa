error_chain! {
    foreign_links {
        Handlebars(::handlebars::TemplateRenderError);
        Io(::std::io::Error);
    }
}
