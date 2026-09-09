(defrule probe =>
 (printout t (str-index "" a) ":" (str-index "" abc-def) ":" (str-index "" TRUE) ":"
  (str-index "" (sym-cat "")) ":" (str-index (sym-cat "") "abc") crlf))
