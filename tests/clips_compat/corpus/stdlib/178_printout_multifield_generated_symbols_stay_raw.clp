
(defrule exercise =>
(printout t (create$ (sym-cat "") (sym-cat "two words") (sym-cat "a\"b") (sym-cat "a\\b") (sym-cat "(x)") (sym-cat "crlf")) crlf)
)
