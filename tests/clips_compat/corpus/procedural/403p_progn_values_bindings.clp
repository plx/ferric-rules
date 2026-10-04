(defrule run =>
 (printout t (progn) ":" (progn 1 2 3) ":" (progn (bind ?x 7) ?x) crlf)
 (printout t ?x crlf))
