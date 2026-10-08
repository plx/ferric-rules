(deftemplate p (slot x))
(deftemplate q (slot x))
(deffacts d (p (x 1)) (q (x 2)))
(defrule query =>
(assert (p (x 3)))
(do-for-all-facts ((?f (progn (printout t reset crlf) (reset) (create$ p q)))) TRUE (printout t ?f:x crlf))
(halt)
)
