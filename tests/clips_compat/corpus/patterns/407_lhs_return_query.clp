(deftemplate p (slot x))
(deffacts d (p (x 1)) (go 1))
(defrule r (go =(if (any-factp ((?f p)) (eq ?f:x 1)) then 1 else 0)) => (printout t fired crlf))
