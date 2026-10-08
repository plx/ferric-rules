(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(deffunction target (?label ?name) (printout t ?label) ?name)
(defrule query =>
(printout t before (any-factp ((?f (target A p) (target B missing) (target C q))) TRUE) after crlf)
)
