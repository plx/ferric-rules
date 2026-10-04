(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(printout t before (length$ (find-all-facts ((?f (create$))) TRUE)) after crlf)
)
