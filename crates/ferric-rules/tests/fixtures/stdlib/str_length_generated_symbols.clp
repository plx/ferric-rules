(defrule probe =>
  (printout t (str-length (sym-cat "alpha" "-" "beta")) ":"
    (str-length (sym-cat "a b")) crlf))
