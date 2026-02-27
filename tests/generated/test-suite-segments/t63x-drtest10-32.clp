(defrule bug
    (Surname ?surname_1)
    (PersonSurname ?PersonSurname_1)
    (exists 
        (or  
            (and  
                (exists 
                    (Surname ?Surname_2)
                    (LVAR two ?two)
                    (test (eq ?Surname_2 ?two))) 
                (test (eq ?surname_1 ?PersonSurname_1))) 
            (and  
                (exists 
                    (Surname ?Surname_3)
                    (LVAR three ?three)
                    (test (eq ?Surname_3 ?three))))))
=>)
