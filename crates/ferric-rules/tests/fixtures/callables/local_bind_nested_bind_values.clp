(deffunction collect (?x) (bind ?x (bind ?x (+ ?x 1)) ?x))
(defrule probe => (printout t (collect 3) ":" (collect 5) crlf))
