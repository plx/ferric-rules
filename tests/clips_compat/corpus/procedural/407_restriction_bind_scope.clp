(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(do-for-all-facts ((?f (bind ?target p))) (eq ?target p) (printout t ?f:x ":" ?target crlf))
(printout t "after=" ?target crlf)
)
