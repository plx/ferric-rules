(deffacts d (b))
(defrule hit (or (or (a) (b)) (c)) => (printout t hit crlf))
(defrule miss (or (or (x) (y)) (z)) => (printout t miss crlf))
