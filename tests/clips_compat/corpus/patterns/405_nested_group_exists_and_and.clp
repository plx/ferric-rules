(deffacts d (a) (c) (x))
(defrule hit (exists (and (a) (and (c) (x)))) => (printout t hit crlf))
(defrule miss (exists (and (a) (and (c) (y)))) => (printout t miss crlf))
