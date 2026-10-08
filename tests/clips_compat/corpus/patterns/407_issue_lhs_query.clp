(deftemplate p (slot x))
(deffacts d (p (x 1)) (go))
(defrule r (go) (test (any-factp ((?f p)) (eq ?f:x 1))) => (printout t fired crlf))
