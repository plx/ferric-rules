(deffacts d (x) (a))
(defrule hit (x) (not (exists (or (b) (d)))) => (printout t hit crlf))
(defrule miss (x) (not (exists (or (b) (a)))) => (printout t miss crlf))
