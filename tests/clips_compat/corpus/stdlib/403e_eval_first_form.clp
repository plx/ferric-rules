(defrule run =>
  (printout t (eval "(+ 1 2) (unknown-function)") ":"
    (eval "42 )") ":" (eval "word \"unfinished") crlf))
