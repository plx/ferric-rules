(deffunction f (?x-index)
 (bind ?x-index 90)
 (progn$ (?x (create$ a b))
  (printout t ?x-index ":" (bind ?x-index 9) ":" ?x-index crlf))
 ?x-index)
(defrule probe =>
  (bind ?result (f 80))
  (printout t "after:" ?result crlf))
