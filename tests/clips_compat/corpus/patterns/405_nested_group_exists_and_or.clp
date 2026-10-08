(deffacts d (a) (b))
(defrule hit (exists (and (or (a) (b)))) => (printout t hit crlf))
(defrule miss (exists (and (or (x) (y)))) => (printout t miss crlf))
