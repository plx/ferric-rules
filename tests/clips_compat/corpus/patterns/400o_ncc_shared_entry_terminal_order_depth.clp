(deffacts seed (p))
(defrule r1 (p) (not (and (x) (y))) => (printout t r1 crlf))
(defrule r2 (p) => (printout t r2 crlf))
(defrule r3 (p) (not (and (x) (z))) => (printout t r3 crlf))
