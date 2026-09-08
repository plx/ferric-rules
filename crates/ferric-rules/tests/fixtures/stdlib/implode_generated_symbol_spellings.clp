
(defrule probe =>
(printout t "[" (implode$ (create$ (sym-cat "two words") (sym-cat "a\"b") (sym-cat "a\\b") (sym-cat "") (sym-cat "(x)") (sym-cat "crlf") (sym-cat "tab") (sym-cat "ff"))) "]" crlf)
)
