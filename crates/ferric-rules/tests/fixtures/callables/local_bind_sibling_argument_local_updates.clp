(deffunction observe (?x) (+ (bind ?x (+ ?x 1)) ?x))
(defrule probe => (printout t (observe 3) ":" (observe 5) crlf))
