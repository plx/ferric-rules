(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(bind ?target (create$ q p q))
(do-for-all-facts ((?f ?target p)) TRUE (printout t ?f:x crlf))
(printout t "target=" ?target crlf)
)
