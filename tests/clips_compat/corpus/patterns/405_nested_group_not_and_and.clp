(deffacts d (x) (a))
(defrule hit (not (and (x) (and (a) (b)))) => (printout t hit crlf))
(defrule miss (not (and (x) (and (a)))) => (printout t miss crlf))
