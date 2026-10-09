(defglobal ?*a* = 42 ?*b* = (any-factp ((?f missing)) TRUE))
(defrule r => (printout t "a=" ?*a* crlf))
