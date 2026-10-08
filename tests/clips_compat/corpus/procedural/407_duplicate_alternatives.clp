(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(do-for-all-facts ((?f p p q p)) TRUE (printout t ?f:x crlf))
)
