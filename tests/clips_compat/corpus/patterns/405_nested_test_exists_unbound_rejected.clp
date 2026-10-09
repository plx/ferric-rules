(deffacts d (seed) (a 1) (a -1) (b 1) (b 2) (c 1))
(defrule r (seed) (exists (c ?) (test (> ?zz 0))) => (printout t "fired" crlf))
