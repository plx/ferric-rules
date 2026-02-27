(deftemplate attempt
   (multifield numbers (default 7 7 3 3))
   (multifield rpn))
(deffacts initial-info
   (attempt)
   (operator *)
   (operator /)
   (operator -)
   (operator +))
(defrule do-first
   ?f <- (attempt (numbers $?b ?n1 $?m ?n2 $?e)
                  (rpn))
   (operator ?o)
   =>
   (duplicate ?f (numbers ?b ?m ?e)
                 (rpn ?n1 ?n2 ?o)))
(defrule do-next
   ?f <- (attempt (numbers $?b ?n $?e)
                  (rpn ?f $?rest))
   (operator ?o)
   =>
   (duplicate ?f (numbers ?b ?e)
                 (rpn ?f ?rest ?n ?o)))
