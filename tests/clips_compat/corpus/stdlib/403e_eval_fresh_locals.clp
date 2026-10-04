(defrule run =>
  (bind ?x 9)
  (printout t (eval "(bind ?x 4)") " " ?x " ")
  (printout t (eval "(loop-for-count (?i 1 2) (printout t ?i))") crlf))
