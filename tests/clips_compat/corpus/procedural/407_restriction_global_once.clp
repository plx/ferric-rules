(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defglobal ?*target* = p)
(defrule query =>
(do-for-all-facts ((?f ?*target* q)) TRUE (printout t ?f:x crlf) (bind ?*target* q))
(printout t "target=" ?*target* crlf)
)
