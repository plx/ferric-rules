; Bind action is recognized and does not crash execution
; The rule fires, bind executes, and subsequent printout produces output
(deffacts startup (data 10 20))

(defrule compute
    (data ?x ?y)
    =>
    (bind ?sum (+ ?x ?y))
    (printout t "computed" crlf))
