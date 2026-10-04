(deffunction check () (if FALSE then (assert (item 1)) else (printout t (any-factp ((?f item)) TRUE) crlf)))
(defrule r => (check))
