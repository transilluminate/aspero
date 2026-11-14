# Citation styles and locales

The `.csl` styles and `locales-*.xml` files in this directory are from the
Citation Style Language project, unmodified:

- styles: <https://github.com/citation-style-language/styles> at
  `af0aea3fffd44fe798cce9285598d0d5cf62ef23`
- locales: <https://github.com/citation-style-language/locales> at
  `8bef426f89f38c0789223c50e53ec2197bc50bb3`

They are licensed under the Creative Commons Attribution-ShareAlike 3.0
license (<https://creativecommons.org/licenses/by-sa/3.0/>), not under
aspero's own license. Each file names its authors and contributors;
`aspero styles --credits` (and the FFI's `styleCredits`, or as data
`citationStyles` and `styleProject`) lists them.

When a style is loaded, a `<names>` element without a `<name>` child is
given an empty `<name/>`, which the CSL specification implies, to work
around hayagriva 0.10.1 rendering no names there (fixed upstream in
typst/hayagriva#520). The files themselves are unchanged.
