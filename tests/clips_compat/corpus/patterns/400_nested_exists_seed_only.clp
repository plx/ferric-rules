(deffacts seed (seed))
(defrule r2 (seed) (exists (exists (a) (b))) => (printout t "r2 fired" crlf))
(defrule n3 (seed) (not (not (not (a)))) => (printout t "n3 fired" crlf))
