return {
    save_trim_ws = { "*" },
    save_lsp_format = { "*.rs" },
    telescope = {
        file_ignore_patterns = {
            "contrib/",
        }
    },
    spectre = {
        path = "!contrib/**",
    },
    lsp = {
        ['rust-analyzer'] = {},
    }
}

