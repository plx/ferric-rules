(deftemplate p (slot x))
(deftemplate q (slot x))
(deffacts d (p (x 1)) (q (x 2)))
(defrule query =>
(delayed-do-for-all-facts ((?f p q)) TRUE
 (printout t ?f:x crlf)
 (if (= ?f:x 1) then (assert (p (x 3))) (assert (q (x 4)))))
)
