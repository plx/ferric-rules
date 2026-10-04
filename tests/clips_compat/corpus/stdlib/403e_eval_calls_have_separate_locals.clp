(defrule run =>
  (printout t (eval "(bind ?x 4)") ":" (eval "?x") "after" crlf))
