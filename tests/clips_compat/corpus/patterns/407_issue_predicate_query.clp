(deftemplate p (slot x))
(deffacts d (p (x 1)) (go))
(defrule r (go) (p (x ?v&:(any-factp ((?f p)) (eq ?f:x ?v)))) => (printout t fired crlf))
