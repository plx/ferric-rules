(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(deffunction target (?label ?name) (printout t ?label) ?name)
(defrule query =>
(do-for-all-facts ((?f (target A p) (target B q)) (?g (target C q))) TRUE (printout t ":" ?f:x "," ?g:x crlf))
)
