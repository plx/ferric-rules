; call-next-method: more-specific method calls the next-less-specific method.
; INTEGER is more specific than NUMBER; the INTEGER method logs then delegates.
(defgeneric annotate)
(defmethod annotate ((?x NUMBER)) (str-cat "num(" ?x ")"))
(defmethod annotate ((?x INTEGER)) (str-cat "int+" (call-next-method)))

(deffacts startup (run-it))

(defrule dispatch
    (run-it)
    =>
    (printout t (annotate 7) crlf)
    (printout t (annotate 2.5) crlf))
