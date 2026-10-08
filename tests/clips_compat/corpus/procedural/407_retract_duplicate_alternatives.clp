(deftemplate p (slot x))
(deftemplate q (slot x))
(deffacts d (p (x 1)) (q (x 2)))
(defrule query =>
(do-for-all-facts ((?f p p q)) TRUE (printout t ?f:x crlf) (if (= ?f:x 1) then (retract ?f)))
)
