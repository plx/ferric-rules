(deffacts d (a) (c))
(defrule hit (not (exists (a) (or (b) (q)))) => (printout t hit crlf))
(defrule miss (not (exists (a) (or (b) (c)))) => (printout t miss crlf))
