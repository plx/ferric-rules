; Generic dispatch with type-specific methods.
; Most specific method wins: INTEGER > NUMBER for integer args.
(defgeneric describe)
(defmethod describe ((?x INTEGER)) (str-cat "int:" ?x))
(defmethod describe ((?x FLOAT)) (str-cat "float:" ?x))
(defmethod describe ((?x SYMBOL)) (str-cat "sym:" ?x))

(deffacts startup (test-it))

(defrule test-dispatch
    (test-it)
    =>
    (printout t (describe 42) crlf)
    (printout t (describe hello) crlf))
