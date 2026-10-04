(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(do-for-all-facts ((?f p)) TRUE
 (printout t "outer=" ?f:x ":")
 (do-for-all-facts ((?f q)) TRUE (printout t ?f:x ":"))
 (printout t ?f:x crlf))
)
