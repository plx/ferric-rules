(deffacts seed (u 3))
(defrule alternatives (or (u ?) (u ?x&:(and (> ?x 0) (< ?x 10)) )) => (printout t OR crlf))
(defrule middle (u 3) => (printout t MIDDLE crlf))
