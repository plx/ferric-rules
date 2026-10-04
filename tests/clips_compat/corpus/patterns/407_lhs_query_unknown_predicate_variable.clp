(deftemplate p (slot x))
(defrule r (value ?x&:(any-factp ((?f p)) (= ?f:x ?missing))) => (printout t BAD crlf))
