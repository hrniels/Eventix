return {
    save_trim_ws = { "*" },
    save_lsp_format = { "*.rs" },
    save_format_cmd = {
        ['*.yaml'] = 'yamlfmt -',
        ['*.yml'] = 'yamlfmt -',
    },
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

