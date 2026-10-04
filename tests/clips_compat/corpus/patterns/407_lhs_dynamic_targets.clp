(deftemplate p (slot x))
(deftemplate q (slot x))
(deffacts d (p (x 1)) (q (x 2)) (want p 1) (want q 2))
(defrule r (want ?t ?x) (test (any-factp ((?f ?t)) (= ?f:x ?x))) => (printout t ?t ":" ?x crlf))
