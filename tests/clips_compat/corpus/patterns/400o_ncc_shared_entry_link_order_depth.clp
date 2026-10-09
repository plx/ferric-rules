(deffacts seed (b) (c) (d) (a))
(defrule r1 (a) (b) (c) => (printout t r1 crlf))
(defrule r2 (a) (d) => (printout t r2 crlf))
(defrule r3 (a) (not (and (b) (e))) => (printout t r3 crlf))
