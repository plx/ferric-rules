(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(deffunction target (?label ?name) (printout t ?label) ?name)
(defrule query =>
(printout t "empty=" (length$ (find-all-facts ((?f empty) (?g (target E p))) TRUE)) crlf)
(printout t "any=" (any-factp ((?f (target A p) (target B q)) (?g (target C p))) TRUE) crlf)
)
