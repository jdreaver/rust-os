;; Fixes rust-analyzer complaining about a lack of a crate for `test`. See
;; https://github.com/rust-lang/rust-analyzer/issues/3801. Getting the actual
;; target seems to work, thanks to
;; https://github.com/rust-lang/rust-analyzer/pull/8774, but we still need to
;; disable _all_ targets to avoid the error.
((rust-mode . ((lsp-rust-analyzer-check-all-targets . nil))))
