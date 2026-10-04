(deftemplate p (slot x))
(defrule r (go) (test (any-factp ((?f p)) (= ?f:x ?missing))) => (printout t BAD crlf))
