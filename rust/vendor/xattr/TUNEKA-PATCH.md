# Local compatibility patch

Source: crates.io xattr 0.2.3, https://github.com/Stebalien/xattr (Steven Allen), MIT OR Apache-2.0. Both original license files and upstream sources/tests are retained.

GPUI 0.2.2 depends on zed-async-tar 0.5.0-zed, which requires xattr 0.2.3. On Linux/Android, libc 0.2.190 removed the non-native ENOATTR alias. The only source change maps this crate's internal ENOATTR constant to libc::ENODATA for those targets; other operating systems are unchanged. See https://github.com/rust-lang/libc/issues/1356 . Remove this override when GPUI upgrades the archive dependency to a current xattr.
