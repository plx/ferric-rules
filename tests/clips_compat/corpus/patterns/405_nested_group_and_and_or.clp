(deffacts d (a))
(defrule hit (and (and (or (a) (b)))) => (printout t hit crlf))
(defrule miss (and (and (or (x) (y)))) => (printout t miss crlf))
