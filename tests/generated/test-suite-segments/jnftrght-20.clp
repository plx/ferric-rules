(defrule problem-rule-5
   (A ?td2)  
   (not (and (not (and (B) 
                       (C ?td2)))  
             (not (and (D)
                       (not (E ?td2))))))
   =>)
