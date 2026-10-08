(deffacts d (b) (c))
(defrule hit (or (and (or (a) (b)) (c)) (d)) => (printout t hit crlf))
(defrule miss (or (and (or (a) (b)) (e)) (f)) => (printout t miss crlf))
