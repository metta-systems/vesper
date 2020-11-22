The different libraries seem to target different use-cases, though. For example, `snafu` with its
strongly typed errors and contexts seems to be a good fit for libraries. On the other hand,
anyhow with its focus on the type-erased Error and on creating string errors and contexts seems
to be more useful for applications. After all, errors produced by libraries need to be understood
by other code, errors produced by executables need to be understood by humans.
-- https://lukaskalbertodt.github.io/2019/11/14/thoughts-on-error-handling-in-rust.html

@see also https://blog.yoshuawuyts.com/error-handling-survey/
@see also https://www.reddit.com/r/rust/comments/dfs1zk/2019_q4_error_patterns_snafu_vs_errderive_anyhow/
