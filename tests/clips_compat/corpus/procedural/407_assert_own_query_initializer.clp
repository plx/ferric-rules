(defrule r => (bind ?made (assert (item (any-factp ((?f item)) TRUE)))) (printout t (fact-slot-value ?made implied) crlf))
