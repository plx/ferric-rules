; Generic function dispatch by argument type.
; Method bodies are pure expressions — printout lives in the rule RHS.
(defgeneric describe)
(defmethod describe ((?x INTEGER)) (str-cat "integer: " ?x))
(defmethod describe ((?x STRING)) (str-cat "string: " ?x))

(deffacts startup (test-int) (test-str))

(defrule test-integer
    (test-int)
    =>
    (printout t (describe 42) crlf))

(defrule test-string
    (test-str)
    =>
    (printout t (describe "hello") crlf))
