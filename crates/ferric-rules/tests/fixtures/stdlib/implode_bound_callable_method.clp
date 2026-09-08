(deffunction render (?fields) (implode$ ?fields))
(defgeneric render-method)
(defmethod render-method ((?fields MULTIFIELD)) (implode$ ?fields))
(deffacts startup (payload a "b c" 2.5))
(defrule probe (payload $?fields) =>
(printout t "[" (render ?fields) "]" crlf)
(printout t "[" (render-method ?fields) "]" crlf)
)
